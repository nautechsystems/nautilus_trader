import pickle

from nautilus_trader.common.events import TimeEvent
from nautilus_trader.core.uuid import UUID4


def test_time_event_equality():
    # Arrange
    event_id = UUID4()

    event1 = TimeEvent(
        "TEST_EVENT",
        event_id,
        1,
        2,
    )

    event2 = TimeEvent(
        "TEST_EVENT",
        event_id,
        1,
        2,
    )

    event3 = TimeEvent(
        "TEST_EVENT",
        UUID4(),
        1,
        2,
    )

    # Act, Assert
    assert event1.name == event2.name == event3.name
    assert event1 == event2
    assert event3 != event1
    assert event3 != event2


def test_time_event_picking():
    # Arrange
    event = TimeEvent(
        "TEST_EVENT",
        UUID4(),
        1,
        2,
    )

    # Act
    pickled = pickle.dumps(event)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle is safe here)

    # Assert
    assert event == unpickled
