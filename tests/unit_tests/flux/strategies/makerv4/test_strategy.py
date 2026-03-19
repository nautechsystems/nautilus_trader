from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_BALANCES
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_ALERT
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_STATE
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_TRADE
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.flux.strategies.makerv4.managed_orders import HedgeBacklogState
from nautilus_trader.flux.strategies.makerv4.managed_orders import ManagedMakerOrderState
from nautilus_trader.flux.strategies.makerv4.wire import HedgeExecutionReport
from nautilus_trader.flux.strategies.makerv4.wire import MakerFill
from nautilus_trader.flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.adapters.interactive_brokers.common import IB_CLIENT_ID
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs

_REGULAR_SESSION_TS_MS = 1_742_223_600_000
_REGULAR_SESSION_TS_NS = 1_742_223_600_000_000_000
_OVERNIGHT_TS_MS = 1_742_176_800_000
_OVERNIGHT_TS_NS = 1_742_176_800_000_000_000


def _config(**overrides) -> MakerV4StrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_makerv4",
        "strategy_id": "aapl_tradexyz_makerv4",
        "outside_rth_hedge_enabled": True,
    }
    base.update(overrides)
    return MakerV4StrategyConfig(**base)


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
    fill_id: str = "fill-1",
    side: str = "BUY",
    qty: str = "2",
    px: str = "190.00",
    ts_ms: int = _REGULAR_SESSION_TS_MS,
) -> MakerFill:
    return MakerFill(
        fill_id=fill_id,
        side=side,
        qty=Decimal(qty),
        price=Decimal(px),
        ts_ms=ts_ms,
    )


def _fill_event(
    *,
    instrument_id,
    fill_id: str = "fill-1",
    client_order_id: str = "maker-1",
    side: str = "BUY",
    qty: str = "2",
    px: str = "190.00",
    ts_event: int = _REGULAR_SESSION_TS_NS,
    commission: object | None = None,
):
    return SimpleNamespace(
        instrument_id=instrument_id,
        trade_id=fill_id,
        client_order_id=client_order_id,
        order_side=side,
        last_qty=Decimal(qty),
        last_px=Decimal(px),
        ts_event=ts_event,
        commission=commission,
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


def _install_limit_order_factory(strategy: MakerV4Strategy, monkeypatch) -> list[SimpleNamespace]:
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


def _configure_strategy_for_quoting(strategy: MakerV4Strategy) -> tuple[InstrumentId, InstrumentId]:
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
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


def _prepare_strategy_for_fill_events(strategy: MakerV4Strategy) -> MakerV4Strategy:
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": _REGULAR_SESSION_TS_NS - 50_000_000,
        },
    }
    strategy._submit_hedge_intent = lambda _intent: "hedge-fill-1"
    strategy._publish_json = lambda *_args, **_kwargs: None
    return strategy


class _StubAccount:
    def __init__(self, *, account_id: str, balances: dict[str, str]) -> None:
        self.id = account_id
        self._balances = {currency: Decimal(value) for currency, value in balances.items()}

    def balances_total(self) -> dict[str, Decimal]:
        return dict(self._balances)

    def balances_free(self) -> dict[str, Decimal]:
        return dict(self._balances)

    def balances_locked(self) -> dict[str, Decimal]:
        return {currency: Decimal("0") for currency in self._balances}


class _StubReferenceBalanceProvider:
    def __init__(self, snapshot: dict[str, object]) -> None:
        self._snapshot = snapshot
        self.started = 0
        self.stopped = 0

    def start(self, *, strategy: MakerV4Strategy) -> None:
        self.started += 1

    def stop(self) -> None:
        self.stopped += 1

    def snapshot(self) -> dict[str, object]:
        return self._snapshot


def test_makerv4_fill_builds_ioc_hedge_order_through_mid() -> None:
    strategy = MakerV4Strategy(config=_config())

    order = strategy.record_maker_fill(
        fill=_fill(side="SELL"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    assert order is not None
    assert order.side == "BUY"
    assert order.qty == Decimal("2")
    assert order.limit_price == Decimal("190.04")
    assert order.time_in_force == "IOC"
    assert order.outside_rth is True


def test_makerv4_submit_hedge_intent_attaches_outside_rth_ibkr_tag(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    created = _install_limit_order_factory(strategy, monkeypatch)
    submit_calls: list[dict[str, object]] = []
    strategy.submit_order = lambda _order, **kwargs: submit_calls.append(kwargs)
    strategy._instruments = {
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    order_id = strategy._submit_hedge_intent(
        strategy.record_maker_fill(
            fill=_fill(side="SELL"),
            quote=_quote(),
            maker_fee_bps=Decimal("0.25"),
        ),
    )

    assert order_id == "order-1"
    assert len(created) == 1
    assert created[0].tags == [IBOrderTags(outsideRth=True).value]
    assert submit_calls == [{"client_id": IB_CLIENT_ID}]


def test_makerv4_submit_hedge_intent_does_not_force_include_overnight_during_regular_session(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config(ibkr_hedge_route="BLUEOCEAN"))
    created = _install_limit_order_factory(strategy, monkeypatch)
    strategy.submit_order = lambda _order, **_kwargs: None
    strategy._instruments = {
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    order_id = strategy._submit_hedge_intent(
        strategy.record_maker_fill(
            fill=_fill(side="SELL"),
            quote=_quote(),
            maker_fee_bps=Decimal("0.25"),
        ),
    )

    assert order_id == "order-1"
    assert len(created) == 1
    assert created[0].tags == [IBOrderTags(outsideRth=True).value]


def test_makerv4_regular_session_hedge_policy_keeps_immediate_ioc_without_overnight_tags(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(
        config=_config(
            outside_rth_hedge_enabled=False,
            ibkr_hedge_route="SMART",
        ),
    )
    created = _install_limit_order_factory(strategy, monkeypatch)
    strategy.submit_order = lambda _order, **_kwargs: None
    strategy._instruments = {
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    order_id = strategy._submit_hedge_intent(
        SimpleNamespace(
            instrument_id=str(strategy.config.reference_instrument_id),
            side="BUY",
            qty=Decimal("2"),
            limit_price=Decimal("190.04"),
            time_in_force="IOC",
            outside_rth=False,
            include_overnight=False,
            route="SMART",
            cancel_after_ms=None,
        ),
    )

    assert order_id == "order-1"
    assert len(created) == 1
    assert _enum_name(created[0].time_in_force) == "IOC"
    assert created[0].tags is None


def test_makerv4_overnight_hedge_policy_keeps_immediate_ioc_and_include_overnight(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config(ibkr_hedge_route="SMART"))
    created = _install_limit_order_factory(strategy, monkeypatch)
    strategy.submit_order = lambda _order, **_kwargs: None
    strategy._instruments = {
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )

    order_id = strategy._submit_hedge_intent(
        SimpleNamespace(
            instrument_id=str(strategy.config.reference_instrument_id),
            side="BUY",
            qty=Decimal("2"),
            limit_price=Decimal("190.04"),
            time_in_force="IOC",
            outside_rth=True,
            include_overnight=True,
            route="SMART",
            cancel_after_ms=None,
        ),
    )

    assert order_id == "order-1"
    assert len(created) == 1
    assert _enum_name(created[0].time_in_force) == "IOC"
    assert created[0].tags == [IBOrderTags(outsideRth=True, includeOvernight=True).value]


def test_makerv4_pending_hedge_policy_metadata_uses_overnight_day_policy() -> None:
    strategy = MakerV4Strategy(config=_config(ibkr_hedge_route="SMART"))

    strategy.record_maker_fill(
        fill=_fill(fill_id="fill-overnight-policy", ts_ms=_OVERNIGHT_TS_MS),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    snapshot = strategy.snapshot_state()
    pending = snapshot.get("pending_hedge", {})

    assert pending.get("route") == "SMART"
    assert pending.get("time_in_force") == "DAY"
    assert pending.get("include_overnight") is True
    assert pending.get("cancel_after_ms") == 5_000


def test_makerv4_overnight_blueocean_config_normalizes_to_smart_hedge_instrument() -> None:
    strategy = MakerV4Strategy(config=_config(ibkr_hedge_route="BLUEOCEAN"))

    order = strategy.record_maker_fill(
        fill=_fill(
            fill_id="fill-blueocean-overnight",
            side="SELL",
            ts_ms=_OVERNIGHT_TS_MS,
        ),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    assert order is not None
    assert order.route == "SMART"
    assert order.time_in_force == "DAY"
    assert order.outside_rth is True
    assert order.include_overnight is True
    assert order.cancel_after_ms == 5_000
    assert order.instrument_id == str(strategy.config.reference_instrument_id)
    assert strategy._last_pricing_debug["hedge_instrument_id"] == str(
        strategy.config.reference_instrument_id,
    )


def test_makerv4_on_order_filled_maker_event_creates_one_hedge_request() -> None:
    strategy = _prepare_strategy_for_fill_events(MakerV4Strategy(config=_config()))

    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.maker_instrument_id,
            side="SELL",
        )
    )

    assert strategy.hedge_request_count == 1
    assert strategy.pending_hedge_qty == Decimal("2")
    assert strategy._pending_hedge is not None
    assert strategy._pending_hedge.order_id == "hedge-fill-1"


def test_makerv4_on_order_filled_maker_event_publishes_trade_payload() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    published: list[tuple[str, dict[str, object]]] = []

    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": _REGULAR_SESSION_TS_NS - 50_000_000,
        },
    }
    strategy._managed_maker_orders = {
        "SELL": ManagedMakerOrderState(
            client_order_id="maker-1",
            instrument_id=str(maker_id),
            side="SELL",
            quantity=Decimal("2"),
            price=Decimal("190.00"),
            post_only=True,
        ),
    }
    strategy._submit_hedge_intent = lambda _intent: "hedge-fill-1"
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="maker-fill-1",
            side="SELL",
            commission=SimpleNamespace(
                as_decimal=lambda: Decimal("0.00098500"),
                currency=SimpleNamespace(code="USDC"),
            ),
        )
    )

    trade_payloads = [payload for topic, payload in published if topic == TOPIC_TRADE]
    assert len(trade_payloads) == 1
    assert trade_payloads[0]["strategy_id"] == "aapl_tradexyz_makerv4"
    assert trade_payloads[0]["trade_role"] == "maker"
    assert trade_payloads[0]["instrument_id"] == str(maker_id)
    assert trade_payloads[0]["trade_id"] == "maker-fill-1"
    assert trade_payloads[0]["coin"] == "AAPL"
    assert trade_payloads[0]["exchange"] == "hyperliquid"
    assert trade_payloads[0]["fee_amount_raw"] == "0.00098500"
    assert trade_payloads[0]["fee_asset_raw"] == "USDC"


def test_makerv4_on_order_filled_maker_event_accepts_client_order_id_objects() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    published: list[tuple[str, dict[str, object]]] = []

    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": _REGULAR_SESSION_TS_NS - 50_000_000,
        },
    }
    strategy._managed_maker_orders = {
        "BUY": ManagedMakerOrderState(
            client_order_id="maker-1",
            instrument_id=str(maker_id),
            side="BUY",
            quantity=Decimal("2"),
            price=Decimal("190.00"),
            post_only=True,
        ),
    }
    strategy._submit_hedge_intent = lambda _intent: "hedge-fill-1"
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            client_order_id=ClientOrderId("maker-1"),
            qty="2",
            px="190.00",
        )
    )

    trade_payloads = [payload for topic, payload in published if topic == TOPIC_TRADE]
    assert len(trade_payloads) == 1
    assert trade_payloads[0]["trade_role"] == "maker"
    assert strategy.hedge_request_count == 1
    assert strategy.pending_hedge_qty == Decimal("2")


def test_makerv4_second_maker_fill_while_hedge_pending_fails_closed_without_overwrite() -> None:
    strategy = _prepare_strategy_for_fill_events(MakerV4Strategy(config=_config()))

    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.maker_instrument_id,
            fill_id="fill-pending-1",
            qty="1",
        )
    )
    first_pending = strategy._pending_hedge

    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.maker_instrument_id,
            fill_id="fill-pending-2",
            qty="1",
            ts_event=_REGULAR_SESSION_TS_NS + 1_000_000,
        )
    )

    assert first_pending is not None
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "pending_hedge_exists"
    assert strategy._pending_hedge == first_pending
    assert strategy.hedge_request_count == 1


def test_makerv4_duplicate_fill_event_does_not_double_hedge() -> None:
    strategy = MakerV4Strategy(config=_config())

    first = strategy.record_maker_fill(
        fill=_fill(fill_id="fill-1"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )
    duplicate = strategy.record_maker_fill(
        fill=_fill(fill_id="fill-1"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    assert first is not None
    assert duplicate is None
    assert strategy.hedge_request_count == 1


def test_makerv4_duplicate_fill_event_is_ignored_through_order_callback() -> None:
    strategy = _prepare_strategy_for_fill_events(MakerV4Strategy(config=_config()))
    event = _fill_event(instrument_id=strategy.config.maker_instrument_id, fill_id="fill-1")

    strategy.on_order_filled(event)
    strategy.on_order_filled(event)

    assert strategy.hedge_request_count == 1
    assert strategy.pending_hedge_qty == Decimal("2")


def test_makerv4_pauses_when_ibkr_quote_is_stale() -> None:
    strategy = MakerV4Strategy(config=_config())

    order = strategy.record_maker_fill(
        fill=_fill(),
        quote=_quote(age_ms=2_000),
        maker_fee_bps=Decimal("0.25"),
    )

    assert order is None
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "stale_quote"


def test_makerv4_take_take_stale_reference_quote_creates_recoverable_hedge_backlog(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
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
            "execution_mode": "take_take",
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
    assert strategy._hedge_backlog is None
    assert strategy._pending_hedge is not None
    assert strategy.hedge_disabled_reason is None
    assert strategy.tradeable is True


def test_makerv4_partial_hedge_fill_pauses_strategy() -> None:
    strategy = MakerV4Strategy(config=_config())
    strategy.record_maker_fill(
        fill=_fill(qty="3"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    strategy.apply_hedge_execution(
        HedgeExecutionReport(
            order_id="hedge-1",
            ok=True,
            filled_qty=Decimal("1"),
            avg_fill_price=Decimal("190.04"),
            error=None,
        )
    )

    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "partial_hedge_fill"
    assert strategy.pending_hedge_qty == Decimal("2")


def test_makerv4_partial_hedge_fill_via_order_callback_pauses_strategy() -> None:
    strategy = _prepare_strategy_for_fill_events(MakerV4Strategy(config=_config()))
    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.maker_instrument_id,
            fill_id="fill-2",
            side="BUY",
            qty="3",
        )
    )

    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.reference_instrument_id,
            fill_id="hedge-fill-2-partial",
            client_order_id="hedge-fill-2",
            side="SELL",
            qty="1",
            px="190.04",
            ts_event=1_100_000_000,
        )
    )

    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "partial_hedge_fill"
    assert strategy.pending_hedge_qty == Decimal("2")


def test_makerv4_snapshot_restores_pending_hedge_state() -> None:
    strategy = MakerV4Strategy(config=_config())
    strategy.record_maker_fill(
        fill=_fill(fill_id="fill-77", qty="1"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )
    snapshot = strategy.snapshot_state()

    restored = MakerV4Strategy(config=_config())
    restored.restore_state(snapshot)

    assert restored.pending_hedge_qty == Decimal("1")
    assert restored.snapshot_state()["last_fill_ids_head"] == ["fill-77"]


def test_makerv4_snapshot_restores_hedge_backlog_state() -> None:
    strategy = MakerV4Strategy(config=_config())
    strategy._hedge_backlog = HedgeBacklogState(
        fill_id="fill-backlog-1",
        side="SELL",
        requested_qty=Decimal("2"),
        blocked_reason="stale_quote",
        fill_ts_ms=2_000,
        maker_fee_bps=Decimal("0.25"),
    )
    strategy.tradeable = False
    strategy.hedge_disabled_reason = "stale_quote"

    snapshot = strategy.snapshot_state()
    restored = MakerV4Strategy(config=_config())
    restored.restore_state(snapshot)

    assert restored._hedge_backlog is not None
    assert restored._hedge_backlog.fill_id == "fill-backlog-1"
    assert restored._hedge_backlog.requested_qty == Decimal("2")
    assert restored._hedge_backlog.blocked_reason == "stale_quote"
    assert restored.tradeable is False
    assert restored.hedge_disabled_reason == "stale_quote"


def test_makerv4_restore_does_not_resubmit_pending_hedge_on_reconnect() -> None:
    strategy = _prepare_strategy_for_fill_events(MakerV4Strategy(config=_config()))
    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.maker_instrument_id,
            fill_id="fill-77",
            qty="1",
        )
    )
    snapshot = strategy.snapshot_state()
    restored = MakerV4Strategy(config=_config())
    maker_id = restored.config.maker_instrument_id
    ref_id = restored.config.reference_instrument_id
    instruments = {
        maker_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL"),
    }
    restored.restore_state(snapshot)
    restored._cache = SimpleNamespace(instrument=lambda instrument_id: instruments.get(instrument_id))
    restored.subscribe_quote_ticks = lambda *_args, **_kwargs: None
    restored._publish_balances = lambda: None
    restored._publish_state_snapshot = lambda **_kwargs: None

    restored.on_start()
    restored.on_order_filled(
        _fill_event(
            instrument_id=restored.config.maker_instrument_id,
            fill_id="fill-77",
            qty="1",
        )
    )

    assert restored.pending_hedge_qty == Decimal("1")
    assert restored.hedge_request_count == 0


def test_makerv4_partial_maker_fill_keeps_side_managed_until_terminal_fill(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    strategy._runtime_params["qty"] = Decimal("2")
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    strategy._submit_hedge_intent = lambda _intent: "hedge-fill-partial"
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            client_order_id="order-1",
            fill_id="maker-partial-1",
            qty="1",
            ts_event=2_002_000_000,
        )
    )

    buy_state = strategy._managed_maker_orders.get("BUY")
    assert buy_state is not None
    assert buy_state.client_order_id == "order-1"
    assert buy_state.quantity == Decimal("1")
    assert strategy.pending_hedge_qty == Decimal("1")

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            client_order_id="order-1",
            fill_id="maker-partial-2",
            qty="1",
            ts_event=2_003_000_000,
        )
    )

    assert "BUY" not in strategy._managed_maker_orders


def test_makerv4_on_start_reclaims_open_maker_orders_and_skips_duplicate_requote(monkeypatch) -> None:
    source = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(source)
    source_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    source_submitted: list[SimpleNamespace] = []

    source._publish_market_bbo = lambda **_kwargs: None
    source._publish_balances_if_due = lambda: None
    source._publish_state_snapshot = lambda **_kwargs: None
    source.submit_order = lambda order, **_kwargs: source_submitted.append(order)
    monkeypatch.setattr(type(source), "clock", property(lambda _self: source_clock))
    _install_limit_order_factory(source, monkeypatch)

    source.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    source.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    assert [order.client_order_id for order in source_submitted] == ["order-1", "order-2"]

    restored = MakerV4Strategy(config=_config())
    restored_submitted: list[SimpleNamespace] = []
    restored_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    restored._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    restored._cache = SimpleNamespace(
        instrument=lambda instrument_id: restored._instruments.get(instrument_id),
        orders_open=lambda **_kwargs: list(source_submitted),
        orders_inflight=lambda **_kwargs: [],
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    restored.subscribe_quote_ticks = lambda *_args, **_kwargs: None
    restored._publish_market_bbo = lambda **_kwargs: None
    restored._publish_balances_if_due = lambda: None
    restored._publish_balances = lambda: None
    restored._publish_state_snapshot = lambda **_kwargs: None
    restored.submit_order = lambda order, **_kwargs: restored_submitted.append(order)
    monkeypatch.setattr(type(restored), "clock", property(lambda _self: restored_clock))
    _install_limit_order_factory(restored, monkeypatch)

    restored.on_start()

    assert list(restored._managed_maker_orders) == ["BUY", "SELL"]
    assert restored._managed_maker_orders["BUY"].client_order_id == "order-1"
    assert restored._managed_maker_orders["SELL"].client_order_id == "order-2"

    restored.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    restored.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    assert restored_submitted == []


def test_makerv4_on_start_subscribes_quote_ticks_and_publishes_initial_snapshots() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    instruments = {
        maker_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL/USD"),
    }
    subscribed: list[InstrumentId] = []
    published: list[str] = []

    strategy._cache = SimpleNamespace(instrument=lambda instrument_id: instruments.get(instrument_id))
    strategy.subscribe_quote_ticks = lambda instrument_id, **_kwargs: subscribed.append(instrument_id)
    strategy._publish_balances = lambda: published.append("balances")
    strategy._publish_state_snapshot = lambda **_kwargs: published.append("state")

    strategy.on_start()

    assert subscribed == [maker_id, ref_id]
    assert published == ["balances", "state"]


def test_makerv4_on_quote_tick_publishes_market_bbo() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL/USD"),
    }
    strategy._last_market_bbo_publish_ns = {maker_id: 0}
    published: list[dict[str, object]] = []
    state_updates: list[int] = []

    strategy._publish_market_bbo = lambda **kwargs: published.append(kwargs)
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda *, now_ns=None: state_updates.append(int(now_ns or 0))

    tick = SimpleNamespace(
        instrument_id=maker_id,
        bid_price=SimpleNamespace(as_decimal=lambda: Decimal("190.00")),
        ask_price=SimpleNamespace(as_decimal=lambda: Decimal("190.04")),
        ts_event=2_000_000_000,
    )

    strategy.on_quote_tick(tick)

    assert published == [
        {
            "instrument_id": maker_id,
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 2_000_000_000,
        },
    ]
    assert state_updates == [2_000_000_000]


def test_makerv4_disable_hedging_stops_quote_target_generation() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.04"), "ts_ns": 2_000_000_000},
        ref_id: {"bid": Decimal("189.98"), "ask": Decimal("190.02"), "ts_ns": 2_000_000_000},
    }

    assert strategy._maker_quote_targets(now_ns=2_001_000_000) is not None

    strategy._disable_hedging("hedge_rejected")

    assert strategy._maker_quote_targets(now_ns=2_001_000_000) is None


def test_makerv4_disable_hedging_cancels_resting_maker_quotes(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    submitted: list[SimpleNamespace] = []
    canceled: list[InstrumentId] = []

    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._refresh_runtime_params_if_due = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    strategy.cancel_all_orders = lambda instrument_id: canceled.append(instrument_id)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    assert len(submitted) == 2
    assert all(state.pending_cancel is False for state in strategy._managed_maker_orders.values())

    strategy._disable_hedging("hedge_rejected")

    assert canceled == [maker_id]
    assert all(state.pending_cancel is True for state in strategy._managed_maker_orders.values())


def test_makerv4_blocked_state_does_not_generate_new_quotes(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy._disable_hedging("hedge_rejected")
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert submitted == []
    assert payload["state"] == "blocked_hedge_rejected"
    assert payload["maker_quote_status"] == {
        "bid_open": 0,
        "ask_open": 0,
        "bid_depth": 0,
        "ask_depth": 0,
        "bid_blocked": 0,
        "ask_blocked": 0,
    }
    assert payload["maker_v4"]["managed_maker_orders"] == []


def test_makerv4_on_quote_tick_places_initial_two_sided_maker_quotes_when_fresh_and_bot_on(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._refresh_runtime_params_if_due = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    assert len(submitted) == 2
    assert {str(order.instrument_id) for order in submitted} == {str(maker_id)}
    assert {_enum_name(order.side) for order in submitted} == {"BUY", "SELL"}
    assert all(order.post_only is True for order in submitted)


def test_makerv4_on_quote_tick_refreshes_runtime_bot_on_without_restart(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params["bot_on"] = False
    strategy._params_manager = SimpleNamespace(
        load=lambda: {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
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
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    assert strategy._effective_bot_on() is True
    assert len(submitted) == 2


def test_makerv4_on_quote_tick_refreshes_runtime_bot_off_and_cancels_managed_orders(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    cancel_requests: list[object] = []

    strategy._params_manager = SimpleNamespace(load=lambda: {"bot_on": False})
    strategy._managed_maker_orders = {
        "BUY": SimpleNamespace(
            client_order_id="maker-buy-1",
            instrument_id=str(maker_id),
            side="BUY",
            quantity=Decimal("1"),
            price=Decimal("190.00"),
            post_only=True,
            pending_cancel=False,
        )
    }
    strategy.cancel_all_orders = lambda instrument_id: cancel_requests.append(instrument_id)
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    assert strategy._effective_bot_on() is False
    assert cancel_requests == [maker_id]
    assert strategy._managed_maker_orders["BUY"].pending_cancel is True


def test_makerv4_take_take_suppresses_resting_maker_quote_placement(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
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

    assert len(submitted) == 1
    assert str(submitted[0].instrument_id) == str(maker_id)
    assert _enum_name(submitted[0].side) == "BUY"
    assert submitted[0].post_only is False
    assert strategy._pending_hedge is None
    assert strategy.hedge_request_count == 0


def test_makerv4_take_take_submits_aggressive_hl_order_only_when_threshold_met(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
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
            bid="189.97",
            ask="189.98",
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

    assert submitted == []

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.18",
            ask="189.20",
            ts_event=2_002_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_003_000_000,
        )
    )

    assert len(submitted) == 1
    assert str(submitted[0].instrument_id) == str(maker_id)


def test_makerv4_take_take_blocks_when_maker_quote_is_old(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "max_age_ms": 1_000,
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
            ts_event=900_000_000,
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

    assert submitted == []


def test_makerv4_take_take_blocks_unhedgeable_qty_before_hl_aggression(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config(hedge_min_share_increment=Decimal("10")))
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "qty": Decimal("5"),
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

    assert submitted == []
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "hedge_qty_rounds_to_zero"
    assert strategy._pending_hedge is None
    assert strategy.hedge_request_count == 0


def test_makerv4_take_take_submits_ibkr_hedge_only_after_hl_fill_callback(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
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


def test_makerv4_take_take_multi_fill_callbacks_submit_one_hedge_for_total_filled_qty(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "qty": Decimal("2"),
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

    assert len(submitted) == 1
    assert strategy._pending_hedge is None
    assert strategy.tradeable is True
    assert strategy.hedge_disabled_reason is None
    assert strategy._managed_maker_orders["BUY"].quantity == Decimal("1")

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-2",
            client_order_id="order-1",
            side="BUY",
            qty="1",
            px="189.19",
            ts_event=2_003_000_000,
        )
    )

    assert len(submitted) == 2
    hedge_order = submitted[1]
    assert str(hedge_order.instrument_id) == str(ref_id)
    assert _enum_name(hedge_order.side) == "SELL"
    assert hedge_order.quantity == Decimal("2")


def test_makerv4_take_take_accumulates_sub_share_terminal_fills_across_orders_until_hedgeable(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "qty": Decimal("1"),
        }
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)
    strategy._latest_quotes = {
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_999_000_000,
        },
    }

    strategy._managed_maker_orders["SELL"] = ManagedMakerOrderState(
        client_order_id="order-1",
        instrument_id=str(maker_id),
        side="SELL",
        quantity=Decimal("1"),
        price=Decimal("189.20"),
        post_only=False,
    )
    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-1",
            client_order_id="order-1",
            side="SELL",
            qty="0.6",
            px="189.20",
            ts_event=2_002_000_000,
        )
    )
    strategy.on_order_canceled(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            ts_event=2_003_000_000,
        )
    )

    assert submitted == []
    assert strategy._pending_hedge is None
    assert strategy.tradeable is True
    assert strategy.hedge_disabled_reason is None

    strategy._managed_maker_orders["SELL"] = ManagedMakerOrderState(
        client_order_id="order-2",
        instrument_id=str(maker_id),
        side="SELL",
        quantity=Decimal("1"),
        price=Decimal("189.18"),
        post_only=False,
    )
    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-2",
            client_order_id="order-2",
            side="SELL",
            qty="0.4",
            px="189.18",
            ts_event=2_004_000_000,
        )
    )
    strategy.on_order_canceled(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-2",
            ts_event=2_005_000_000,
        )
    )

    assert len(submitted) == 1
    hedge_order = submitted[0]
    assert str(hedge_order.instrument_id) == str(ref_id)
    assert _enum_name(hedge_order.side) == "BUY"
    assert hedge_order.quantity == Decimal("1")
    assert strategy._pending_hedge is not None
    assert strategy.hedge_disabled_reason is None


def test_makerv4_take_take_base_qty_converts_hl_venue_size_and_hedges_full_base_fill(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config(qty_unit="base"))
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "qty": Decimal("1"),
        }
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)
    strategy._instruments[maker_id] = _instrument(raw_symbol="AAPL/USD", multiplier="0.0625")

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

    assert len(submitted) == 1
    assert submitted[0].quantity == Decimal("16")
    assert strategy._managed_maker_orders["BUY"].quantity == Decimal("1")

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
    hedge_order = submitted[1]
    assert str(hedge_order.instrument_id) == str(ref_id)
    assert _enum_name(hedge_order.side) == "SELL"
    assert hedge_order.quantity == Decimal("1")
    assert strategy.tradeable is True
    assert strategy.hedge_disabled_reason is None
    assert strategy.hedge_request_count == 1
    assert strategy._pending_hedge is not None
    assert strategy._pending_hedge.requested_qty == Decimal("1")


def test_makerv4_take_take_reconciles_closed_partial_ioc_orders_without_terminal_callbacks(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    closed_orders: set[str] = set()

    class _FakeClock:
        def __init__(self) -> None:
            self.now = 2_000_000_000

        def timestamp_ns(self) -> int:
            return self.now

    fake_clock = _FakeClock()
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "take_cooldown_ms": 0,
            "qty": Decimal("1"),
        }
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
        order=lambda client_order_id: SimpleNamespace(
            is_closed=lambda: client_order_id in closed_orders,
        ),
    )

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

    assert len(submitted) == 1
    assert submitted[0].client_order_id == "order-1"

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-1",
            client_order_id="order-1",
            side="BUY",
            qty="0.6",
            px="189.20",
            ts_event=2_002_000_000,
        )
    )

    assert strategy._pending_hedge is None
    assert strategy._take_take_residual_base_fill is None
    assert "BUY" in strategy._managed_maker_orders

    closed_orders.add("order-1")
    fake_clock.now = 2_003_000_000
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.17",
            ask="189.19",
            ts_event=2_003_000_000,
        )
    )

    assert strategy._take_take_residual_base_fill == {
        "side": "BUY",
        "qty": Decimal("0.6"),
        "ts_ms": 2002,
    }
    assert "BUY" in strategy._managed_maker_orders
    assert strategy._managed_maker_orders["BUY"].client_order_id == "order-2"
    assert len(submitted) == 2

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-2",
            client_order_id="order-2",
            side="BUY",
            qty="0.4",
            px="189.19",
            ts_event=2_004_000_000,
        )
    )
    closed_orders.add("order-2")
    fake_clock.now = 2_005_000_000
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.16",
            ask="189.18",
            ts_event=2_005_000_000,
        )
    )

    assert len(submitted) == 3
    hedge_order = submitted[2]
    assert str(hedge_order.instrument_id) == str(ref_id)
    assert _enum_name(hedge_order.side) == "SELL"
    assert hedge_order.quantity == Decimal("1")
    assert strategy._pending_hedge is not None
    assert strategy._take_take_residual_base_fill is None


def test_makerv4_take_take_late_fill_after_cache_reconcile_keeps_recoverable_stale_quote_backlog(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    closed_orders: set[str] = set()

    class _FakeClock:
        def __init__(self) -> None:
            self.now = 2_000_000_000

        def timestamp_ns(self) -> int:
            return self.now

    fake_clock = _FakeClock()
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "take_cooldown_ms": 0,
            "qty": Decimal("1"),
        }
    )
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
        order=lambda client_order_id: SimpleNamespace(
            is_closed=lambda: client_order_id in closed_orders,
        ),
    )

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

    assert len(submitted) == 1
    assert submitted[0].client_order_id == "order-1"

    closed_orders.add("order-1")
    fake_clock.now = 2_003_000_000
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="189.17",
            ask="189.19",
            ts_event=2_003_000_000,
        )
    )

    assert "BUY" in strategy._managed_maker_orders
    assert strategy._managed_maker_orders["BUY"].client_order_id == "order-2"
    assert strategy._pending_hedge is None
    assert strategy._hedge_backlog is None

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="take-fill-late-1",
            client_order_id="order-1",
            side="BUY",
            qty="1",
            px="189.20",
            ts_event=4_500_000_000,
        )
    )

    assert [order.client_order_id for order in submitted] == ["order-1", "order-2"]
    assert strategy._pending_hedge is None
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "stale_quote"
    assert strategy._hedge_backlog is not None
    assert strategy._hedge_backlog.blocked_reason == "stale_quote"


def test_makerv4_take_take_cooldown_suppresses_immediate_refire(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quoting(strategy)
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_000_000_000)
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "execution_mode": "take_take",
            "bid_edge_take_bps": 5.0,
            "ask_edge_take_bps": 50.0,
            "take_cooldown_ms": 5_000,
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

    assert len(submitted) == 1

    strategy.on_order_expired(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            ts_event=2_002_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_003_000_000,
        )
    )

    assert len(submitted) == 1

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="190.00",
            ask="190.04",
            ts_event=7_100_000_000,
        )
    )

    assert len(submitted) == 2
    assert str(submitted[1].instrument_id) == str(maker_id)


def test_makerv4_state_snapshot_reports_pending_managed_orders() -> None:
    strategy = _prepare_strategy_for_fill_events(MakerV4Strategy(config=_config()))
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy.on_order_filled(
        _fill_event(
            instrument_id=strategy.config.maker_instrument_id,
            fill_id="fill-managed",
            qty="1",
        )
    )

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    assert state_payloads[-1]["managed_orders"] == 1
    assert state_payloads[-1]["tracked_managed_orders"] == 1


def test_makerv4_on_order_filled_hedge_event_publishes_trade_payload() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    published: list[tuple[str, dict[str, object]]] = []

    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": _REGULAR_SESSION_TS_NS - 50_000_000,
        },
    }
    strategy._submit_hedge_intent = lambda _intent: "hedge-fill-1"
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="maker-fill-1",
            side="SELL",
        )
    )
    published.clear()

    strategy.on_order_filled(
        _fill_event(
            instrument_id=ref_id,
            fill_id="hedge-fill-1",
            client_order_id="hedge-fill-1",
            side="BUY",
            qty="2",
            px="190.01",
            commission=SimpleNamespace(
                as_decimal=lambda: Decimal("1.00"),
                currency=SimpleNamespace(code="USD"),
            ),
        )
    )

    trade_payloads = [payload for topic, payload in published if topic == TOPIC_TRADE]
    assert len(trade_payloads) == 1
    assert trade_payloads[0]["strategy_id"] == "aapl_tradexyz_makerv4"
    assert trade_payloads[0]["trade_role"] == "hedge"
    assert trade_payloads[0]["instrument_id"] == str(ref_id)
    assert trade_payloads[0]["trade_id"] == "hedge-fill-1"
    assert trade_payloads[0]["coin"] == "AAPL"
    assert trade_payloads[0]["exchange"] == "nasdaq"
    assert trade_payloads[0]["fee_amount_raw"] == "1.00"
    assert trade_payloads[0]["fee_asset_raw"] == "USD"
    assert trade_payloads[0]["fee_quote"] == "1.00"


def test_makerv4_on_order_filled_submits_real_ioc_hedge_order_without_stub(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: _REGULAR_SESSION_TS_NS)
    submitted: list[SimpleNamespace] = []

    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._latest_quotes = {
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": _REGULAR_SESSION_TS_NS - 50_000_000,
        },
    }
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="fill-live-1",
            side="SELL",
        )
    )

    assert len(submitted) == 1
    hedge_order = submitted[0]
    assert str(hedge_order.instrument_id) == str(ref_id)
    assert _enum_name(hedge_order.side) == "BUY"
    assert _enum_name(hedge_order.time_in_force) == "IOC"
    assert strategy._pending_hedge is not None
    assert strategy._pending_hedge.order_id == hedge_order.client_order_id


def test_makerv4_state_snapshot_arms_pending_hedge_and_cancels_remaining_maker_quotes(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: _REGULAR_SESSION_TS_NS)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []
    canceled: list[object] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    strategy.cancel_all_orders = lambda instrument_id: canceled.append(instrument_id)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=_REGULAR_SESSION_TS_NS,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=_REGULAR_SESSION_TS_NS + 1_000_000,
        )
    )
    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="fill-live-2",
            client_order_id="order-2",
            side="SELL",
            qty="2",
            ts_event=_REGULAR_SESSION_TS_NS + 2_000_000,
        )
    )

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert canceled == [maker_id]
    assert payload["maker_quote_status"] == {
        "bid_open": 1,
        "ask_open": 0,
        "bid_depth": 1,
        "ask_depth": 0,
        "bid_blocked": 0,
        "ask_blocked": 0,
    }
    assert payload["maker_v4"]["managed_maker_orders"] == [
        {
            "client_order_id": "order-1",
            "instrument_id": str(maker_id),
            "side": "BUY",
        },
    ]
    assert payload["maker_v4"]["pending_hedge"] == {
        "client_order_id": "order-3",
        "instrument_id": str(ref_id),
        "route": "SMART",
        "side": "BUY",
        "time_in_force": "IOC",
        "outside_rth": True,
        "include_overnight": False,
        "cancel_after_ms": None,
        "remaining_qty": "2",
    }


def test_makerv4_pending_hedge_blocks_requote_until_terminal_hedge_outcome(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []
    canceled: list[object] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    strategy.cancel_all_orders = lambda instrument_id: canceled.append(instrument_id)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="fill-live-3",
            client_order_id="order-2",
            side="SELL",
            qty="2",
            ts_event=2_002_000_000,
        )
    )
    published.clear()

    assert canceled == [maker_id]

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_003_000_000,
        )
    )

    assert canceled == [maker_id]
    assert [order.client_order_id for order in submitted] == [
        "order-1",
        "order-2",
        "order-3",
    ]

    strategy.on_order_canceled(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            ts_event=2_004_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_005_000_000,
        )
    )

    assert [order.client_order_id for order in submitted] == ["order-1", "order-2", "order-3"]
    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["maker_quote_status"] == {
        "bid_open": 0,
        "ask_open": 0,
        "bid_depth": 0,
        "ask_depth": 0,
        "bid_blocked": 0,
        "ask_blocked": 0,
    }
    assert payload["maker_v4"]["managed_maker_orders"] == []
    assert payload["maker_v4"]["pending_hedge"]["client_order_id"] == "order-3"


def test_makerv4_pending_hedge_retries_cancel_submission_after_cancel_submit_failure(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    submitted: list[SimpleNamespace] = []
    cancel_attempts: list[object] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda *_args, **_kwargs: None  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)

    def cancel_all_orders(instrument_id) -> None:
        cancel_attempts.append(instrument_id)
        if len(cancel_attempts) == 1:
            raise RuntimeError("cancel submit failed")

    strategy.cancel_all_orders = cancel_all_orders
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )

    strategy.on_order_filled(
        _fill_event(
            instrument_id=maker_id,
            fill_id="fill-live-4",
            client_order_id="order-2",
            side="SELL",
            qty="2",
            ts_event=2_002_000_000,
        )
    )

    assert cancel_attempts == [maker_id]
    assert strategy._managed_maker_orders["BUY"].pending_cancel is False

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_003_000_000,
        )
    )

    assert cancel_attempts == [maker_id, maker_id]
    assert strategy._managed_maker_orders["BUY"].pending_cancel is True
    assert [order.client_order_id for order in submitted] == [
        "order-1",
        "order-2",
        "order-3",
    ]


def test_makerv4_maker_order_cancel_reconciles_managed_order_state(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )
    published.clear()

    strategy.on_order_canceled(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            ts_event=2_002_000_000,
        )
    )

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["maker_quote_status"] == {
        "bid_open": 0,
        "ask_open": 1,
        "bid_depth": 0,
        "ask_depth": 1,
        "bid_blocked": 0,
        "ask_blocked": 0,
    }
    assert payload["maker_v4"]["managed_maker_orders"] == [
        {
            "client_order_id": "order-2",
            "instrument_id": str(maker_id),
            "side": "SELL",
        },
    ]


def test_makerv4_maker_order_reject_reconciles_managed_order_state(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )
    published.clear()

    strategy.on_order_rejected(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            reason="post_only_reject",
            ts_event=2_002_000_000,
        )
    )

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["maker_quote_status"]["bid_open"] == 0
    assert payload["maker_quote_status"]["ask_open"] == 1
    assert payload["maker_v4"]["managed_maker_orders"] == [
        {
            "client_order_id": "order-2",
            "instrument_id": str(maker_id),
            "side": "SELL",
        },
    ]


def test_makerv4_maker_order_quota_reject_records_diagnostics_without_latching_blocked(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )
    published.clear()

    raw_reason = (
        "Too many cumulative requests sent (15965 > 15855) for cumulative volume traded "
        "$5856.56. Place taker orders to free up 1 request per USDC traded."
    )
    strategy.on_order_rejected(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            reason=raw_reason,
            ts_event=2_002_000_000,
        )
    )

    assert strategy.tradeable is True
    assert strategy.hedge_disabled_reason is None
    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["state"] == "running"
    assert payload["maker_quote_status"]["bid_open"] == 0
    assert payload["maker_quote_status"]["ask_open"] == 1
    assert payload["pricing_debug"]["venue_protection"]["source_event"] == "order_rejected"
    assert payload["pricing_debug"]["venue_protection"]["quota_requests_used"] == 15965
    assert payload["pricing_debug"]["venue_protection"]["quota_requests_cap"] == 15855
    assert (
        payload["pricing_debug"]["venue_protection"]["quota_cumulative_volume_traded"]
        == "5856.56"
    )
    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert alerts
    assert alerts[-1]["alert_key"] == "venue_protection_circuit_breaker"
    assert alerts[-1]["level"] == "error"
    assert alerts[-1]["actionable"] is True
    assert alerts[-1]["quota_requests_used"] == 15965


def test_makerv4_maker_order_expire_reconciles_managed_order_state(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    submitted: list[SimpleNamespace] = []

    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy.submit_order = lambda order, **_kwargs: submitted.append(order)
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    _install_limit_order_factory(strategy, monkeypatch)

    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=maker_id,
            bid="190.00",
            ask="190.04",
            ts_event=2_000_000_000,
        )
    )
    strategy.on_quote_tick(
        _quote_tick(
            instrument_id=ref_id,
            bid="189.98",
            ask="190.02",
            ts_event=2_001_000_000,
        )
    )
    published.clear()

    strategy.on_order_expired(
        SimpleNamespace(
            instrument_id=maker_id,
            client_order_id="order-1",
            ts_event=2_002_000_000,
        )
    )

    state_payloads = [payload for topic, payload in published if topic == TOPIC_STATE]
    assert state_payloads
    payload = state_payloads[-1]
    assert payload["maker_quote_status"]["bid_open"] == 0
    assert payload["maker_quote_status"]["ask_open"] == 1
    assert payload["maker_v4"]["managed_maker_orders"] == [
        {
            "client_order_id": "order-2",
            "instrument_id": str(maker_id),
            "side": "SELL",
        },
    ]


def test_makerv4_hedge_reject_fails_closed(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 1_200_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    strategy.record_maker_fill(
        fill=_fill(fill_id="fill-reject"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )
    strategy._update_pending_hedge_order_id("hedge-reject-1")

    strategy.on_order_rejected(
        SimpleNamespace(
            instrument_id=strategy.config.reference_instrument_id,
            client_order_id="hedge-reject-1",
            reason="broker_reject",
            ts_event=1_100_000_000,
        )
    )

    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "hedge_rejected"
    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert alerts
    assert alerts[-1]["alert_key"] == "maker_v4_hedge_disabled"
    assert alerts[-1]["hedge_disabled_reason"] == "hedge_rejected"


def test_makerv4_hedge_quota_reject_fails_closed_as_venue_protection(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 1_200_000_000)
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    strategy.record_maker_fill(
        fill=_fill(fill_id="fill-reject"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )
    strategy._update_pending_hedge_order_id("hedge-reject-1")

    raw_reason = (
        "Too many cumulative requests sent (15965 > 15855) for cumulative volume traded "
        "$5856.56. Place taker orders to free up 1 request per USDC traded."
    )
    strategy.on_order_rejected(
        SimpleNamespace(
            instrument_id=strategy.config.reference_instrument_id,
            client_order_id="hedge-reject-1",
            reason=raw_reason,
            ts_event=1_100_000_000,
        )
    )

    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "venue_protection"
    assert strategy._last_venue_protection["quota_requests_used"] == 15965
    assert strategy._last_venue_protection["quota_requests_cap"] == 15855
    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert alerts
    assert alerts[-1]["alert_key"] == "venue_protection_circuit_breaker"
    assert alerts[-1]["level"] == "error"
    assert alerts[-1]["quota_requests_cap"] == 15855


def test_makerv4_hedge_cancel_fails_closed() -> None:
    strategy = MakerV4Strategy(config=_config())
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.record_maker_fill(
        fill=_fill(fill_id="fill-cancel"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )
    strategy._update_pending_hedge_order_id("hedge-cancel-1")

    strategy.on_order_canceled(
        SimpleNamespace(
            instrument_id=strategy.config.reference_instrument_id,
            client_order_id="hedge-cancel-1",
            ts_event=1_100_000_000,
        )
    )

    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "hedge_canceled"


def test_makerv4_hedge_expire_fails_closed_as_timeout() -> None:
    strategy = MakerV4Strategy(config=_config())
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.record_maker_fill(
        fill=_fill(fill_id="fill-expire"),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )
    strategy._update_pending_hedge_order_id("hedge-expire-1")

    strategy.on_order_expired(
        SimpleNamespace(
            instrument_id=strategy.config.reference_instrument_id,
            client_order_id="hedge-expire-1",
            ts_event=1_100_000_000,
        )
    )

    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "hedge_timeout"


def test_makerv4_runtime_params_refresh_failure_publishes_alert(
    monkeypatch,
) -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[tuple[str, dict[str, object]]] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    monkeypatch.setattr(
        strategy,
        "_load_runtime_params",
        lambda: (_ for _ in ()).throw(RuntimeError("boom")),
    )

    strategy._refresh_runtime_params_if_due(now_ns=1_000_000_000, force=True)

    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert alerts
    assert alerts[-1]["alert_key"] == "runtime_params_failure"
    assert alerts[-1]["level"] == "error"
    assert "RuntimeError: boom" in str(alerts[-1]["message"])


def test_makerv4_market_exit_attempt_publishes_critical_alert(monkeypatch) -> None:
    strategy = MakerV4Strategy(config=_config())
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 2_100_000_000)
    published: list[tuple[str, dict[str, object]]] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._open_positions = lambda: [object(), object()]  # type: ignore[assignment]
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))

    strategy.on_market_exit()

    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert alerts
    assert alerts[-1]["alert_key"] == "market_exit_attempt"
    assert alerts[-1]["level"] == "critical"
    assert alerts[-1]["market_exit"] is True
    assert alerts[-1]["open_positions"] == 2


def test_makerv4_market_exit_fill_publishes_trade_and_critical_alert() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    published: list[tuple[str, dict[str, object]]] = []

    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_state_snapshot = lambda **_kwargs: None

    event = _fill_event(
        instrument_id=maker_id,
        client_order_id="market-exit-maker-1",
        side="SELL",
        qty="1",
        px="190.00",
    )
    event.tags = "MARKET_EXIT"

    strategy.on_order_filled(event)

    trades = [payload for topic, payload in published if topic == TOPIC_TRADE]
    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert trades
    assert trades[-1]["trade_role"] == "maker"
    assert trades[-1]["market_exit"] is True
    assert trades[-1]["fill_context"] == "market_exit"
    assert alerts
    assert alerts[-1]["alert_key"] == "market_exit_fill"
    assert alerts[-1]["level"] == "critical"
    assert alerts[-1]["market_exit"] is True


def test_makerv4_market_exit_denied_publishes_critical_alert() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[tuple[str, dict[str, object]]] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]
    strategy._publish_state_snapshot = lambda **_kwargs: None

    strategy.on_order_denied(
        SimpleNamespace(
            instrument_id=strategy.config.reference_instrument_id,
            client_order_id="market-exit-denied-1",
            reason="MARKET_EXIT_IN_PROGRESS",
            ts_event=1_100_000_000,
        )
    )

    alerts = [payload for topic, payload in published if topic == TOPIC_ALERT]
    assert alerts
    assert alerts[-1]["alert_key"] == "market_exit_denied"
    assert alerts[-1]["level"] == "critical"
    assert alerts[-1]["market_exit"] is True


def test_makerv4_publish_balances_keeps_ibkr_reference_rows_profile_owned() -> None:
    strategy = MakerV4Strategy(config=_config())
    strategy.register(
        trader_id=TestIdStubs.trader_id(),
        portfolio=TestComponentStubs.portfolio(),
        msgbus=TestComponentStubs.msgbus(),
        cache=TestComponentStubs.cache(),
        clock=TestComponentStubs.clock(),
    )
    strategy._cache = SimpleNamespace(
        accounts=lambda: [_StubAccount(account_id="HYPERLIQUID-master", balances={"USDC": "250.5"})],
        positions_open=lambda *args, **kwargs: [],
        instrument=lambda instrument_id: None,
    )
    provider = _StubReferenceBalanceProvider(
        {
            "accounts": [
                {
                    "account_id": "U1234567",
                    "venue": "ibkr",
                    "events": [
                        {
                            "account_id": "U1234567",
                            "venue": "ibkr",
                            "balances": [
                                {
                                    "currency": "USD",
                                    "free": "1000",
                                    "locked": "0",
                                    "total": "1000",
                                },
                            ],
                        },
                    ],
                },
            ],
            "positions": [
                {
                    "exchange": "ibkr",
                    "account_id": "U1234567",
                    "instrument_id": "AAPL.NASDAQ",
                    "quantity": "5",
                    "signed_qty": "5",
                    "side": "LONG",
                },
            ],
        },
    )
    published: list[tuple[str, dict[str, object]]] = []

    strategy.configure_reference_balance_snapshot_provider(provider)
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy._publish_balances()

    balances_payloads = [payload for topic, payload in published if topic == TOPIC_BALANCES]
    assert balances_payloads
    payload = balances_payloads[-1]
    assert any(account.get("account_id") == "HYPERLIQUID-master" for account in payload["accounts"])
    assert not any(
        account.get("account_id") == "U1234567" and account.get("venue") == "ibkr"
        for account in payload["accounts"]
    )
    assert not any(
        position.get("exchange") == "ibkr" and position.get("instrument_id") == "AAPL.NASDAQ"
        for position in payload["positions"]
    )


def test_makerv4_state_snapshot_includes_quote_legs_and_role_map() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(raw_symbol="AAPL/USD"),
    }
    strategy._runtime_params["assumed_hedge_fee_bps"] = 1.5
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.04"), "ts_ns": 1_000_000_000},
        ref_id: {"bid": Decimal("189.98"), "ask": Decimal("190.02"), "ts_ns": 1_500_000_000},
    }
    published: list[tuple[str, dict[str, object]]] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy._publish_state_snapshot(now_ns=2_000_000_000)

    assert len(published) == 1
    _topic, payload = published[0]
    assert payload["strategy_id"] == "aapl_tradexyz_makerv4"
    assert payload["maker_role_map"] == {
        "maker_leg": str(maker_id),
        "ref_leg": str(ref_id),
        "hedge_leg": str(ref_id),
    }
    quote_snapshot = payload["maker_v4"]["quote_snapshot"]
    assert quote_snapshot["maker_leg"]["venue"] == "HYPERLIQUID"
    assert quote_snapshot["maker_leg"]["bid"] == 190.0
    assert quote_snapshot["maker_leg"]["ask"] == 190.04
    assert quote_snapshot["maker_leg"]["age_ms"] == 1000
    assert quote_snapshot["hedge_leg"]["venue"] == "IBKR"
    assert quote_snapshot["ref_leg"]["instrument_id"] == str(ref_id)
    assert quote_snapshot["assumed_hedge_fee_bps"] == 1.5


def test_makerv4_state_snapshot_ignores_negative_quote_placeholders() -> None:
    strategy = MakerV4Strategy(config=_config())
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        ref_id: SimpleNamespace(raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        ref_id: {"bid": Decimal("-1"), "ask": Decimal("-1"), "ts_ns": 2_000_000_000},
    }
    published: list[tuple[str, dict[str, object]]] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy._publish_state_snapshot(now_ns=2_000_000_000)

    quote_snapshot = published[0][1]["maker_v4"]["quote_snapshot"]
    assert "bid" not in quote_snapshot["ref_leg"]
    assert "ask" not in quote_snapshot["ref_leg"]
    assert "mid" not in quote_snapshot["ref_leg"]


def test_makerv4_state_snapshot_prefers_pricing_route_and_populates_spread_telemetry() -> None:
    strategy = MakerV4Strategy(config=_config(ibkr_hedge_route="SMART"))
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("100.00"), "ask": Decimal("101.00"), "ts_ns": 1_000_000_000},
        ref_id: {"bid": Decimal("99.00"), "ask": Decimal("100.00"), "ts_ns": 1_500_000_000},
    }
    strategy._runtime_params["assumed_hedge_fee_bps"] = 1.0
    strategy._last_pricing_debug.update(
        {
            "hedge_route": "BLUEOCEAN",
            "expected_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
            "fee_snapshot_age_s": 9.0,
            "hedge_latency_ms": 45,
            "hedge_slippage_bps_vs_mid": 1.5,
        }
    )
    published: list[tuple[str, dict[str, object]]] = []

    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy._publish_state_snapshot(now_ns=2_000_000_000)

    quote_snapshot = published[0][1]["maker_v4"]["quote_snapshot"]
    ref_mid = (Decimal("99.00") + Decimal("100.00")) / Decimal("2")
    maker_mid = (Decimal("100.00") + Decimal("101.00")) / Decimal("2")
    assert quote_snapshot["hedge_route"] == "BLUEOCEAN"
    assert quote_snapshot["hedge_leg"]["route"] == "BLUEOCEAN"
    assert quote_snapshot["mid_spread_bps"] == pytest.approx(float(((maker_mid - ref_mid) / ref_mid) * Decimal("10000")))
    assert quote_snapshot["arb_bid_spread_bps"] == pytest.approx(float(((Decimal("99.00") - Decimal("101.00")) / ref_mid) * Decimal("10000")))
    assert quote_snapshot["arb_ask_spread_bps"] == pytest.approx(float(((Decimal("100.00") - Decimal("100.00")) / ref_mid) * Decimal("10000")))
    assert quote_snapshot["quoted_spread_bps"] == pytest.approx(50.25125628140704)
    assert quote_snapshot["effective_spread_bps"] == pytest.approx(47.50125628140704)
    assert quote_snapshot["expected_maker_fee_bps"] == 0.25
    assert quote_snapshot["assumed_hedge_fee_bps"] == 1.0
    assert quote_snapshot["fee_snapshot_age_s"] == 9.0
    assert quote_snapshot["hedge_latency_ms"] == 45
    assert quote_snapshot["hedge_slippage_bps_vs_mid"] == 1.5
