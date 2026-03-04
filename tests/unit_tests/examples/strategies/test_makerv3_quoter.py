from decimal import Decimal

from nautilus_trader.examples.strategies import makerv3 as example_wrapper
from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.flux.strategies.makerv3 import MakerV3StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId


def test_example_wrapper_exports_canonical_strategy_surface() -> None:
    assert example_wrapper.MakerV3Strategy is MakerV3Strategy
    assert example_wrapper.MakerV3StrategyConfig is MakerV3StrategyConfig


def test_example_wrapper_contains_no_strategy_logic_helpers() -> None:
    assert not hasattr(example_wrapper, "_coerce_runtime_param_value")
    assert not hasattr(example_wrapper, "build_ladder_place_cancel_levels_from_bps")
    assert not hasattr(example_wrapper, "plan_side_rebalance_actions")


def test_example_wrapper_config_and_strategy_construct() -> None:
    config = example_wrapper.MakerV3StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal("1"),
        bot_on=True,
    )

    strategy = example_wrapper.MakerV3Strategy(config=config)

    assert isinstance(config, MakerV3StrategyConfig)
    assert isinstance(strategy, MakerV3Strategy)


def test_example_wrapper_declares_expected_public_api() -> None:
    assert example_wrapper.__all__ == [
        "MakerV3Strategy",
        "MakerV3StrategyConfig",
    ]
