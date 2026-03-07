from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.flux.strategies.makerv4.wire import HedgeExecutionReport
from nautilus_trader.flux.strategies.makerv4.wire import MakerFill
from nautilus_trader.flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from nautilus_trader.model.identifiers import InstrumentId


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
