from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.strategies.makerv4.publisher import build_quote_snapshot_payload
from nautilus_trader.flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.flux.strategies.makerv4.wire import MakerFill
from nautilus_trader.flux.strategies.shared.quote_snapshot import (
    build_quote_snapshot_payload as shared_build_quote_snapshot_payload,
)
from nautilus_trader.model.identifiers import InstrumentId


def _config(**overrides) -> MakerV4StrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_makerv4",
        "strategy_id": "aapl_tradexyz_makerv4",
        "outside_rth_hedge_enabled": True,
        "ibkr_hedge_route": "BLUEOCEAN",
    }
    base.update(overrides)
    return MakerV4StrategyConfig(**base)


def _instrument(*, raw_symbol: str) -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol=raw_symbol,
        price_precision=2,
        price_increment=Decimal("0.01"),
        base_currency=SimpleNamespace(code="AAPL"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code="USD"),
        multiplier=Decimal("1"),
        is_inverse=False,
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
    )


def test_makerv4_publisher_reuses_shared_quote_snapshot_contract() -> None:
    payload = build_quote_snapshot_payload(
        maker_leg={"venue": "HYPERLIQUID", "symbol": "AAPL/USD"},
        hedge_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        ref_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        mid_spread_bps=2.0,
        arb_bid_spread_bps=14.0,
        arb_ask_spread_bps=-11.0,
        effective_spread_bps=6.5,
        assumed_hedge_fee_bps=1.0,
        fee_assumptions={
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "hl_taker_fee_bps": 4.5,
            "hl_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
        },
    )
    shared_payload = shared_build_quote_snapshot_payload(
        maker_leg={"venue": "HYPERLIQUID", "symbol": "AAPL/USD"},
        hedge_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        ref_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        effective_spread_bps=6.5,
        assumed_hedge_fee_bps=1.0,
    )

    assert payload["maker_leg"]["venue"] == "HYPERLIQUID"
    assert payload["hedge_leg"]["venue"] == "IBKR"
    assert payload["ref_leg"]["venue"] == "IBKR"
    assert payload["mid_spread_bps"] == 2.0
    assert payload["arb_bid_spread_bps"] == 14.0
    assert payload["arb_ask_spread_bps"] == -11.0
    assert payload["effective_spread_bps"] == 6.5
    assert payload["fee_assumptions"] == {
        "ibkr_fee_plan": "tiered",
        "ibkr_fee_min_usd": 0.35,
        "hl_taker_fee_bps": 4.5,
        "hl_maker_fee_bps": 0.25,
        "assumed_hedge_fee_bps": 1.0,
    }
    assert payload["hedge_leg"]["fee_assumptions"] == payload["fee_assumptions"]
    assert shared_payload["effective_spread_bps"] == payload["effective_spread_bps"]


def test_makerv4_strategy_quote_snapshot_uses_distinct_hedge_identity_and_fill_telemetry() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(raw_symbol="AAPL/USD"),
    }
    strategy._latest_quotes = {
        maker_id: {
            "bid": Decimal("189.90"),
            "ask": Decimal("190.10"),
            "ts_ns": 1_000_000_000,
        },
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }
    strategy._publish_json = lambda *_args, **_kwargs: None

    order = strategy.record_maker_fill(
        fill=MakerFill(
            fill_id="fill-1",
            side="BUY",
            qty=Decimal("1"),
            price=Decimal("190.00"),
            ts_ms=1_000,
        ),
        quote=IbkrQuoteSnapshot(
            instrument_id=str(ref_id),
            bid=Decimal("190.00"),
            ask=Decimal("190.04"),
            age_ms=25,
            ts_ms=1_000,
        ),
        maker_fee_bps=Decimal("0.25"),
    )

    assert order is not None
    strategy._update_pending_hedge_order_id("hedge-1")
    strategy.on_order_filled(
        SimpleNamespace(
            instrument_id=ref_id,
            trade_id="hedge-fill-1",
            client_order_id="hedge-1",
            order_side="SELL",
            last_qty=Decimal("1"),
            last_px=Decimal("190.01"),
            ts_event=1_040_000_000,
        )
    )

    payload = strategy._quote_snapshot_payload(now_ns=1_050_000_000)
    quoted_spread_bps = ((Decimal("190.02") - Decimal("190.00")) / Decimal("190.02")) * Decimal(
        "10000"
    )
    hedge_slippage_bps = (
        (Decimal("190.02") - Decimal("190.01")) / Decimal("190.02")
    ) * Decimal("10000")
    effective_spread_bps = (
        quoted_spread_bps - Decimal("0.25") - Decimal("1.0") - hedge_slippage_bps
    )
    maker_mid = (Decimal("189.90") + Decimal("190.10")) / Decimal("2")
    ref_mid = (Decimal("190.00") + Decimal("190.04")) / Decimal("2")
    mid_spread_bps = ((maker_mid - ref_mid) / ref_mid) * Decimal("10000")
    arb_bid_spread_bps = ((Decimal("190.00") - Decimal("190.10")) / ref_mid) * Decimal("10000")
    arb_ask_spread_bps = ((Decimal("189.90") - Decimal("190.04")) / ref_mid) * Decimal("10000")

    assert payload["hedge_leg"]["instrument_id"] == "AAPL.NASDAQ"
    assert payload["ref_leg"]["instrument_id"] == "AAPL.NASDAQ"
    assert payload["hedge_route"] == "SMART"
    assert payload["mid_spread_bps"] == pytest.approx(float(mid_spread_bps))
    assert payload["arb_bid_spread_bps"] == pytest.approx(float(arb_bid_spread_bps))
    assert payload["arb_ask_spread_bps"] == pytest.approx(float(arb_ask_spread_bps))
    assert payload["quoted_spread_bps"] == pytest.approx(float(quoted_spread_bps))
    assert payload["expected_maker_fee_bps"] == 0.25
    assert payload["fee_snapshot_age_s"] == 0.025
    assert payload["hedge_latency_ms"] == 40
    assert payload["hedge_slippage_bps_vs_mid"] == pytest.approx(float(hedge_slippage_bps))
    assert payload["effective_spread_bps"] == pytest.approx(float(effective_spread_bps))


def test_makerv4_real_maker_fill_path_uses_configured_hl_maker_fee_in_exported_telemetry() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._runtime_params.update(
        {
            "hl_maker_fee_bps": 1.75,
            "assumed_hedge_fee_bps": 1.0,
        }
    )
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL/USD"),
    }
    strategy._latest_quotes = {
        maker_id: {
            "bid": Decimal("189.90"),
            "ask": Decimal("190.10"),
            "ts_ns": 1_000_000_000,
        },
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._submit_hedge_intent = lambda _intent: "hedge-1"

    strategy.on_order_filled(
        SimpleNamespace(
            instrument_id=maker_id,
            trade_id="maker-fill-1",
            client_order_id="maker-1",
            order_side="BUY",
            last_qty=Decimal("1"),
            last_px=Decimal("190.00"),
            ts_event=1_020_000_000,
        )
    )

    payload = strategy._quote_snapshot_payload(now_ns=1_050_000_000)

    assert strategy._last_pricing_debug["expected_maker_fee_bps"] == 1.75
    assert payload["expected_maker_fee_bps"] == 1.75
    assert payload["fee_assumptions"]["hl_maker_fee_bps"] == 1.75


def test_makerv4_strategy_does_not_assume_smart_route_without_explicit_route_metadata() -> None:
    strategy = MakerV4Strategy(
        config=MakerV4StrategyConfig(
            maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            order_qty=Decimal("1"),
            external_strategy_id="aapl_tradexyz_makerv4",
            strategy_id="aapl_tradexyz_makerv4",
            outside_rth_hedge_enabled=True,
        )
    )
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(raw_symbol="AAPL/USD"),
    }
    strategy._latest_quotes = {
        maker_id: {
            "bid": Decimal("189.90"),
            "ask": Decimal("190.10"),
            "ts_ns": 1_000_000_000,
        },
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }

    payload = strategy._quote_snapshot_payload(now_ns=1_050_000_000)

    assert "hedge_route" not in payload
    assert "route" not in payload["hedge_leg"]


def test_makerv4_strategy_state_snapshot_surfaces_fee_assumptions_in_state_and_quote_exports() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._instruments = {
        maker_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(raw_symbol="AAPL/USD"),
    }
    strategy._runtime_params.update(
        {
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "hl_taker_fee_bps": 4.5,
            "hl_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
        }
    )
    strategy._latest_quotes = {
        maker_id: {
            "bid": Decimal("189.90"),
            "ask": Decimal("190.10"),
            "ts_ns": 1_000_000_000,
        },
        ref_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }
    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_json = lambda topic, payload: published.append((topic, payload))  # type: ignore[assignment]

    strategy._publish_state_snapshot(now_ns=1_050_000_000)

    state_payload = published[-1][1]["maker_v4"]
    quote_snapshot = state_payload["quote_snapshot"]
    expected_fee_assumptions = {
        "ibkr_fee_plan": "tiered",
        "ibkr_fee_min_usd": 0.35,
        "hl_taker_fee_bps": 4.5,
        "hl_maker_fee_bps": 0.25,
        "assumed_hedge_fee_bps": 1.0,
    }

    assert state_payload["fee_assumptions"] == expected_fee_assumptions
    assert quote_snapshot["fee_assumptions"] == expected_fee_assumptions
    assert quote_snapshot["hedge_leg"]["fee_assumptions"] == expected_fee_assumptions


def test_makerv4_quote_targets_move_when_hl_maker_fee_assumption_changes() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
            "bid_edge1": 5.0,
            "ask_edge1": 5.0,
            "hl_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
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
    strategy._latest_quotes = {
        maker_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
        ref_id: {
            "bid": Decimal("189.98"),
            "ask": Decimal("190.02"),
            "ts_ns": 1_000_000_000,
        },
    }

    base_targets = strategy._maker_quote_targets(now_ns=1_001_000_000)
    assert base_targets is not None

    strategy._runtime_params["hl_maker_fee_bps"] = 2.25
    wider_fee_targets = strategy._maker_quote_targets(now_ns=1_001_000_000)

    assert wider_fee_targets is not None
    assert wider_fee_targets["BUY"] < base_targets["BUY"]
    assert wider_fee_targets["SELL"] > base_targets["SELL"]


def test_makerv4_quote_targets_apply_ibkr_fee_plan_minimum_commission_floor() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._runtime_params.update(
        {
            "bot_on": True,
            "n_orders1": 1,
            "n_orders2": 0,
            "n_orders3": 0,
            "qty": 1.0,
            "bid_edge1": 5.0,
            "ask_edge1": 5.0,
            "hl_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.0,
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.0,
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
    strategy._latest_quotes = {
        maker_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
        ref_id: {
            "bid": Decimal("189.98"),
            "ask": Decimal("190.02"),
            "ts_ns": 1_000_000_000,
        },
    }

    no_floor_targets = strategy._maker_quote_targets(now_ns=1_001_000_000)
    assert no_floor_targets is not None

    strategy._runtime_params["ibkr_fee_min_usd"] = 0.35
    tiered_floor_targets = strategy._maker_quote_targets(now_ns=1_001_000_000)
    assert tiered_floor_targets is not None

    strategy._runtime_params["ibkr_fee_plan"] = "fixed"
    fixed_floor_targets = strategy._maker_quote_targets(now_ns=1_001_000_000)

    assert fixed_floor_targets is not None
    assert tiered_floor_targets["BUY"] < no_floor_targets["BUY"]
    assert tiered_floor_targets["SELL"] > no_floor_targets["SELL"]
    assert fixed_floor_targets["BUY"] < tiered_floor_targets["BUY"]
    assert fixed_floor_targets["SELL"] > tiered_floor_targets["SELL"]
