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


def test_makerv4_publisher_reuses_shared_quote_snapshot_contract() -> None:
    assert build_quote_snapshot_payload is shared_build_quote_snapshot_payload

    payload = build_quote_snapshot_payload(
        maker_leg={"venue": "HYPERLIQUID", "symbol": "AAPL/USD"},
        hedge_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        ref_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        effective_spread_bps=6.5,
        assumed_hedge_fee_bps=1.0,
    )

    assert payload["maker_leg"]["venue"] == "HYPERLIQUID"
    assert payload["hedge_leg"]["venue"] == "IBKR"
    assert payload["ref_leg"]["venue"] == "IBKR"
    assert payload["effective_spread_bps"] == 6.5


def test_makerv4_strategy_quote_snapshot_uses_distinct_hedge_identity_and_fill_telemetry() -> None:
    strategy = MakerV4Strategy(config=_config())
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    hedge_id = InstrumentId.from_str("AAPL.BLUEOCEAN")
    strategy._instruments = {
        maker_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        ref_id: SimpleNamespace(raw_symbol="AAPL/USD"),
        hedge_id: SimpleNamespace(raw_symbol="AAPL/USD"),
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
        hedge_id: {
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
            instrument_id=hedge_id,
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

    assert payload["hedge_leg"]["instrument_id"] == "AAPL.BLUEOCEAN"
    assert payload["ref_leg"]["instrument_id"] == "AAPL.NASDAQ"
    assert payload["hedge_route"] == "BLUEOCEAN"
    assert payload["quoted_spread_bps"] == pytest.approx(float(quoted_spread_bps))
    assert payload["expected_maker_fee_bps"] == 0.25
    assert payload["fee_snapshot_age_s"] == 0.025
    assert payload["hedge_latency_ms"] == 40
    assert payload["hedge_slippage_bps_vs_mid"] == pytest.approx(float(hedge_slippage_bps))
    assert payload["effective_spread_bps"] == pytest.approx(float(effective_spread_bps))


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
