from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.strategies.shared.equities_arb import core as core_mod
from nautilus_trader.flux.strategies.shared.equities_arb.hedging import HedgeBacklogState
from nautilus_trader.flux.strategies.shared.equities_arb.hedging import PendingHedgeState
from nautilus_trader.flux.strategies.shared.equities_arb.hedging import build_hedge_backlog_payload
from nautilus_trader.flux.strategies.shared.equities_arb.hedging import build_hedge_policy_payload
from nautilus_trader.flux.strategies.shared.equities_arb.hedging import build_pending_hedge_payload
from nautilus_trader.flux.strategies.shared.equities_arb.instruments import (
    hyperliquid_perp_to_ibkr_instrument_id,
)
from nautilus_trader.flux.strategies.shared.equities_arb.instruments import (
    translate_hyperliquid_fill_to_ibkr_shares,
)
from nautilus_trader.flux.strategies.shared.equities_arb.observability import (
    build_effective_ibkr_fee_bps,
)
from nautilus_trader.flux.strategies.shared.equities_arb.observability import (
    build_fee_assumptions,
)
from nautilus_trader.flux.strategies.shared.equities_arb.observability import (
    build_fee_assumptions_payload,
)
from nautilus_trader.flux.strategies.shared.equities_arb.observability import (
    build_quote_snapshot_payload,
)
from nautilus_trader.flux.strategies.shared.equities_arb.observability import (
    build_take_take_limit_price,
)
from nautilus_trader.flux.strategies.shared.equities_arb.reference_balances import (
    IbkrReferenceBalanceSnapshotProviderConfig,
)
from nautilus_trader.flux.strategies.shared.equities_arb.reference_balances import (
    get_cached_ibkr_reference_balance_provider,
)


def test_runtime_params_module_follows_immediate_hedge_capability() -> None:
    immediate_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )
    maker_spec = SimpleNamespace(
        param_set="future_equities_only_maker",
        capabilities=SimpleNamespace(supports_immediate_hedge=False),
    )

    assert core_mod.runtime_params_module_for_strategy(immediate_spec).PARAM_SET == "makerv4"
    assert core_mod.runtime_params_module_for_strategy(maker_spec).PARAM_SET == "makerv3"


def test_strategy_allowed_instrument_ids_follow_immediate_hedge_capability() -> None:
    immediate_spec = SimpleNamespace(
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )
    maker_spec = SimpleNamespace(
        capabilities=SimpleNamespace(supports_immediate_hedge=False),
    )

    assert core_mod.strategy_allowed_instrument_ids(
        strategy_spec=immediate_spec,
        maker_instrument_id="maker-id",
        reference_instrument_id="ref-id",
    ) == ["maker-id", "ref-id"]
    assert core_mod.strategy_allowed_instrument_ids(
        strategy_spec=maker_spec,
        maker_instrument_id="maker-id",
        reference_instrument_id="ref-id",
    ) == ["maker-id"]


def test_effective_venue_resolution_config_uses_capabilities_not_param_set() -> None:
    strategy_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )
    config = {
        "identity": {
            "strategy_id": "aapl_tradexyz_future",
            "external_strategy_id": "aapl_tradexyz_future",
        },
        "strategy": {
            "ibkr_primary_exchange": "NASDAQ",
        },
        "node": {
            "venues": {
                "HYPERLIQUID": {
                    "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "execution": True,
                },
                "IBKR": {
                    "adapter": "interactive_brokers",
                    "instrument_id": "AAPL.SMART",
                    "execution": False,
                    "ibg_client_id": "",
                },
            },
        },
        "account_scopes": [
            {
                "scope_id": "ibkr.reference.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4002,
                "ibg_client_id": 107,
                "account_id": "U10015777",
            },
        ],
        "strategy_contracts": [
            {
                "strategy_id": "aapl_tradexyz_future",
                "portfolio_asset_id": "AAPL",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "AAPL.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.reference.main",
            },
        ],
    }

    effective = core_mod.effective_venue_resolution_config(
        config=config,
        strategy_spec=strategy_spec,
    )

    assert effective["node"]["venues"]["IBKR"]["instrument_id"] == "AAPL.NASDAQ"
    assert effective["node"]["venues"]["IBKR"]["execution"] is True
    assert effective["node"]["venues"]["IBKR"]["ibg_client_id"] == 107
    assert effective["node"]["venues"]["IBKR"]["account_id"] == "U10015777"


def test_build_pending_hedge_payload_matches_runner_contract() -> None:
    payload = build_pending_hedge_payload(
        PendingHedgeState(
            fill_id="fill-1",
            side="BUY",
            requested_qty=Decimal("2"),
            remaining_qty=Decimal("1"),
            limit_price=Decimal("190.01"),
            route="SMART",
            time_in_force="IOC",
            outside_rth=True,
            include_overnight=False,
            cancel_after_ms=None,
            order_id="hedge-1",
        ),
        hedge_instrument_id="AAPL.NASDAQ",
        decimal_to_json=lambda value: str(value),
    )

    assert payload == {
        "client_order_id": "hedge-1",
        "instrument_id": "AAPL.NASDAQ",
        "route": "SMART",
        "side": "BUY",
        "time_in_force": "IOC",
        "outside_rth": True,
        "include_overnight": False,
        "cancel_after_ms": None,
        "remaining_qty": "1",
    }


def test_build_hedge_backlog_payload_matches_runner_contract() -> None:
    payload = build_hedge_backlog_payload(
        HedgeBacklogState(
            fill_id="take_take:order-1",
            side="SELL",
            requested_qty=Decimal("3"),
            blocked_reason="stale_quote",
            fill_ts_ms=1_700_000_000_000,
            maker_fee_bps=Decimal("0.25"),
        ),
        decimal_to_json=lambda value: str(value),
    )

    assert payload == {
        "fill_id": "take_take:order-1",
        "side": "SELL",
        "requested_qty": "3",
        "blocked_reason": "stale_quote",
        "fill_ts_ms": 1_700_000_000_000,
        "maker_fee_bps": "0.25",
    }


def test_build_hedge_policy_payload_preserves_outside_rth_contract() -> None:
    policy = build_hedge_policy_payload(
        configured_route="BLUEOCEAN",
        outside_rth_enabled=True,
        is_regular_session=False,
        hedge_mode="take_take",
    )

    assert policy == {
        "route": "SMART",
        "time_in_force": "DAY",
        "outside_rth": True,
        "include_overnight": True,
        "cancel_after_ms": 5_000,
    }


def test_build_quote_snapshot_payload_carries_fee_assumptions_on_shared_contract() -> None:
    assumptions = build_fee_assumptions(
        ibkr_fee_plan="tiered",
        ibkr_fee_min_usd=Decimal("0.35"),
        hl_taker_fee_bps=Decimal("4.5"),
        hl_maker_fee_bps=Decimal("0.25"),
        assumed_hedge_fee_bps=Decimal("1.0"),
    )

    payload = build_quote_snapshot_payload(
        maker_leg={"venue": "HYPERLIQUID", "symbol": "AAPL/USD"},
        hedge_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        ref_leg={"venue": "IBKR", "symbol": "AAPL/USD"},
        mid_spread_bps=2.0,
        arb_bid_spread_bps=14.0,
        arb_ask_spread_bps=-11.0,
        effective_spread_bps=6.5,
        assumed_hedge_fee_bps=1.0,
        fee_assumptions=build_fee_assumptions_payload(assumptions),
    )

    assert payload["fee_assumptions"]["ibkr_fee_plan"] == "tiered"
    assert payload["hedge_leg"]["fee_assumptions"] == payload["fee_assumptions"]
    assert payload["mid_spread_bps"] == 2.0
    assert payload["arb_bid_spread_bps"] == 14.0
    assert payload["arb_ask_spread_bps"] == -11.0


def test_instrument_pricing_and_reference_balance_helpers_share_equities_arb_contract() -> None:
    assert translate_hyperliquid_fill_to_ibkr_shares(
        fill_qty=Decimal("1.87"),
        min_share_increment=Decimal("1"),
    ) == Decimal("1")
    assert (
        hyperliquid_perp_to_ibkr_instrument_id(
            "xyz:AAPL-USD-PERP.HYPERLIQUID",
            primary_exchange="NASDAQ",
        )
        == "AAPL.NASDAQ"
    )

    assumptions = build_fee_assumptions(
        ibkr_fee_plan="tiered",
        ibkr_fee_min_usd=Decimal("0.35"),
        hl_taker_fee_bps=Decimal("4.50"),
        hl_maker_fee_bps=Decimal("0.25"),
        assumed_hedge_fee_bps=Decimal("1.00"),
    )
    hedge_fee_bps = build_effective_ibkr_fee_bps(
        fee_assumptions=assumptions,
        hedge_notional_usd=Decimal("190.02"),
    )
    assert build_take_take_limit_price(
        side="BUY",
        maker_bid=Decimal("189.18"),
        maker_ask=Decimal("189.20"),
        reference_bid=Decimal("190.00"),
        reference_ask=Decimal("190.04"),
        target_edge_bps=Decimal("5.0"),
        hl_taker_fee_bps=assumptions.hl_taker_fee_bps,
        hedge_fee_bps=hedge_fee_bps,
    ) == Decimal("189.20")

    config = IbkrReferenceBalanceSnapshotProviderConfig(ibg_client_id=7)
    assert get_cached_ibkr_reference_balance_provider(config) is get_cached_ibkr_reference_balance_provider(config)
