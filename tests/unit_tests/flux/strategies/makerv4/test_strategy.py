from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_BALANCES
from nautilus_trader.flux.strategies.makerv3.constants import TOPIC_STATE
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.flux.strategies.makerv4.wire import HedgeExecutionReport
from nautilus_trader.flux.strategies.makerv4.wire import MakerFill
from nautilus_trader.flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
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


def _fill(*, fill_id: str = "fill-1", side: str = "BUY", qty: str = "2", px: str = "190.00") -> MakerFill:
    return MakerFill(
        fill_id=fill_id,
        side=side,
        qty=Decimal(qty),
        price=Decimal(px),
        ts_ms=1_000,
    )


def _fill_event(
    *,
    instrument_id,
    fill_id: str = "fill-1",
    client_order_id: str = "maker-1",
    side: str = "BUY",
    qty: str = "2",
    px: str = "190.00",
    ts_event: int = 1_000_000_000,
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


def _prepare_strategy_for_fill_events(strategy: MakerV4Strategy) -> MakerV4Strategy:
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(price_precision=2, raw_symbol="AAPL"),
    }
    strategy._latest_quotes = {
        ref_id: {"bid": Decimal("190.00"), "ask": Decimal("190.04"), "ts_ns": 950_000_000},
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


def test_makerv4_publish_balances_includes_reference_snapshot_rows() -> None:
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
    assert any(
        account.get("account_id") == "U1234567" and account.get("venue") == "ibkr"
        for account in payload["accounts"]
    )
    assert any(
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
    assert quote_snapshot["hedge_route"] == "BLUEOCEAN"
    assert quote_snapshot["hedge_leg"]["route"] == "BLUEOCEAN"
    assert quote_snapshot["quoted_spread_bps"] == pytest.approx(50.25125628140704)
    assert quote_snapshot["effective_spread_bps"] == pytest.approx(47.50125628140704)
    assert quote_snapshot["expected_maker_fee_bps"] == 0.25
    assert quote_snapshot["assumed_hedge_fee_bps"] == 1.0
    assert quote_snapshot["fee_snapshot_age_s"] == 9.0
    assert quote_snapshot["hedge_latency_ms"] == 45
    assert quote_snapshot["hedge_slippage_bps_vs_mid"] == 1.5
