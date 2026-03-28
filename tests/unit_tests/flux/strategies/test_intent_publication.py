from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.execution.controller import VenueActivityOrigin
from nautilus_trader.flux.execution.events import ExecutionLifecycleEvent
from nautilus_trader.flux.execution.intents import ExecutionLifecycleState
from nautilus_trader.flux.execution.intents import build_client_order_id
from nautilus_trader.flux.strategies.makerv4.managed_orders import ManagedMakerOrderState
from nautilus_trader.flux.strategies.makerv4.strategy import ControllerIntentCommand
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId


def _config(**overrides) -> MakerV4StrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_makerv4",
        "strategy_id": "aapl_tradexyz_makerv4",
        "controller_scope_id": "equities.ibkr.hedge.main",
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
        info={},
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
    )


def _working_event(command: ControllerIntentCommand, *, controller_seq: int) -> ExecutionLifecycleEvent:
    client_order_id = build_client_order_id(
        controller_scope_id=command.intent.controller_scope_id,
        controller_epoch=9,
        controller_seq=controller_seq,
        intent_id=command.intent.intent_id,
    )
    return ExecutionLifecycleEvent(
        intent_id=command.intent.intent_id,
        controller_scope_id=command.intent.controller_scope_id,
        strategy_id=command.intent.strategy_id,
        controller_epoch=9,
        controller_seq=controller_seq,
        client_order_id=client_order_id,
        venue_order_id=f"VENUE-{controller_seq}",
        lifecycle_state=ExecutionLifecycleState.WORKING,
        venue_activity_origin=VenueActivityOrigin.CONTROLLER,
    )


def test_makerv4_place_actions_publish_controller_intents_before_shadow_state_exists() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: _instrument(raw_symbol="AAPL/USD"),
        strategy.config.reference_instrument_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
    )
    strategy.submit_order = lambda *_args, **_kwargs: pytest.fail(
        "submit_order should not be used on controller-managed canary lanes",
    )

    strategy._submit_maker_quote(side="BUY", target_price=Decimal("190.01"))

    assert len(published) == 1
    command = published[0]
    assert command.command_type == "place"
    assert command.order_role == "maker"
    assert command.instrument_id == str(strategy.config.maker_instrument_id)
    assert command.side == "BUY"
    assert command.quantity == "1"
    assert command.limit_price == "190.01"
    assert strategy._managed_maker_orders == {}

    strategy.apply_controller_lifecycle_event(_working_event(command, controller_seq=11))

    assert strategy._managed_maker_orders["BUY"].client_order_id == build_client_order_id(
        controller_scope_id="equities.ibkr.hedge.main",
        controller_epoch=9,
        controller_seq=11,
        intent_id=command.intent.intent_id,
    )
    assert strategy._managed_maker_orders["BUY"].price == Decimal("190.01")


def test_makerv4_cancel_actions_publish_controller_intents_and_wait_for_callbacks() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy.cancel_all_orders = lambda *_args, **_kwargs: pytest.fail(
        "cancel_all_orders should not be used on controller-managed canary lanes",
    )
    strategy._managed_maker_orders = {
        "BUY": ManagedMakerOrderState(
            client_order_id="managed-buy-1",
            instrument_id=str(strategy.config.maker_instrument_id),
            side="BUY",
            quantity=Decimal("1"),
            price=Decimal("190.01"),
            post_only=True,
        ),
    }

    strategy._cancel_managed_maker_orders()

    assert len(published) == 1
    command = published[0]
    assert command.command_type == "cancel"
    assert command.target_client_order_id == "managed-buy-1"
    assert strategy._managed_maker_orders["BUY"].pending_cancel is False

    strategy.apply_controller_lifecycle_event(_working_event(command, controller_seq=17))

    assert strategy._managed_maker_orders["BUY"].pending_cancel is True


def test_makerv4_hedge_actions_publish_controller_intents_and_store_canonical_exposure() -> None:
    strategy = MakerV4Strategy(config=_config())
    published: list[ControllerIntentCommand] = []
    strategy.configure_controller_intent_publisher(
        controller_scope_id="equities.ibkr.hedge.main",
        publish_intent=published.append,
    )
    strategy.submit_order = lambda *_args, **_kwargs: pytest.fail(
        "submit_order should not be used on controller-managed canary lanes",
    )

    result = strategy._submit_hedge_intent(
        SimpleNamespace(
            instrument_id=str(strategy.config.reference_instrument_id),
            side="BUY",
            qty=Decimal("2"),
            limit_price=Decimal("190.04"),
            time_in_force="IOC",
            outside_rth=True,
            include_overnight=False,
            route="SMART",
            cancel_after_ms=None,
        ),
    )

    assert result is None
    assert len(published) == 1
    command = published[0]
    assert command.command_type == "place"
    assert command.order_role == "hedge"
    assert command.instrument_id == str(strategy.config.reference_instrument_id)
    assert strategy._pending_hedge is None

    strategy.apply_controller_lifecycle_event(_working_event(command, controller_seq=23))
    strategy.apply_controller_canonical_state(
        {
            "net_base_qty": "12",
            "authority_state": "authoritative",
            "stale": False,
        },
    )

    assert strategy._pending_hedge is not None
    assert strategy.snapshot_state()["controller_canonical_state"] == {
        "net_base_qty": "12",
        "authority_state": "authoritative",
        "stale": False,
    }
