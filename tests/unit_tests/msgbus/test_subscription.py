from nautilus_trader.common.component import Subscription


def test_comparisons_returns_expected():
    # Arrange
    subscriber = []
    subscription1 = Subscription(topic="*", handler=subscriber.append, priority=0)
    subscription2 = Subscription(topic="*", handler=subscriber.append, priority=1)

    # Act, Assert
    assert subscription1 == subscription2
    assert subscription1 < subscription2
    assert subscription1 <= subscription2
    assert subscription2 > subscription1
    assert subscription2 >= subscription1


def test_equality_when_equal_returns_true():
    # Arrange
    subscriber = []
    subscription1 = Subscription(topic="*", handler=subscriber.append, priority=1)
    subscription2 = Subscription(topic="*", handler=subscriber.append, priority=2)

    # Act, Assert
    assert subscription1 == subscription2


def test_equality_when_not_equal_returns_false():
    # Arrange
    subscriber = []
    subscription1 = Subscription(topic="*", handler=subscriber.append, priority=1)
    subscription2 = Subscription(topic="something", handler=subscriber.append, priority=2)

    # Act, Assert
    assert subscription1 != subscription2


def test_reverse_sorting_list_of_subscribers_returns_expected_ordered_list():
    # Arrange
    subscriber = []
    subscription1 = Subscription(topic="*", handler=subscriber.append)
    subscription2 = Subscription(topic="*", handler=subscriber.append, priority=5)
    subscription3 = Subscription(topic="*", handler=subscriber.append, priority=2)
    subscription4 = Subscription(topic="*", handler=subscriber.append, priority=10)

    # Act
    sorted_list = sorted([subscription1, subscription2, subscription3, subscription4], reverse=True)

    # Assert
    assert sorted_list == [subscription4, subscription2, subscription3, subscription1]
    assert sorted_list[0] == subscription4
    assert sorted_list[1] == subscription2
    assert sorted_list[2] == subscription3
    assert sorted_list[3] == subscription1


def test_subscription_for_all():
    # Arrange
    subscriber = []
    handler_str = str(subscriber.append)

    # Act
    subscription = Subscription(topic="*", handler=subscriber.append)

    # Assert
    assert str(subscription).startswith(f"Subscription(topic=*, handler={handler_str}, priority=0)")


def test_str_repr():
    # Arrange
    subscriber = []
    handler_str = str(subscriber.append)

    # Act
    subscription = Subscription(topic="system_status", handler=subscriber.append)

    # Assert
    assert (
        str(subscription) == f"Subscription(topic=system_status, handler={handler_str}, priority=0)"
    )
    assert (
        repr(subscription)
        == f"Subscription(topic=system_status, handler={handler_str}, priority=0)"
    )
