import pytest

from nautilus_trader.common.config import InstrumentProviderConfig
from nautilus_trader.config import ActorFactory
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.test_kit.mocks.actors import MockActor


@pytest.mark.parametrize(
    "filters",
    [
        None,
        {},
        {"A": 1, "B": 2, "C": 3},
    ],
)
def test_instrument_provider_config_hash(filters: dict | None) -> None:
    # Arrange
    config = InstrumentProviderConfig(filters=filters)

    # Act
    result = hash(config)

    # Assert
    assert isinstance(result, int)


def test_create_actor_from_importable_config() -> None:
    # Arrange
    config = {
        "component_id": "MyActor",
    }
    importable = ImportableActorConfig(
        actor_path="nautilus_trader.test_kit.mocks.actors:MockActor",
        config_path="nautilus_trader.test_kit.mocks.actors:MockActorConfig",
        config=config,
    )

    # Act
    actor = ActorFactory.create(importable)

    # Assert
    assert isinstance(actor, MockActor)
    assert (
        repr(actor.config)
        == "MockActorConfig(component_id='MyActor', log_events=True, log_commands=True)"
    )
