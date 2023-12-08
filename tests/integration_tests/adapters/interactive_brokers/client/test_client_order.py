from unittest import mock

from ibapi.order import Order as IBOrder


@mock.patch(
    "nautilus_trader.adapters.interactive_brokers.client.InteractiveBrokersClient._eclient.placeOrder",
)
def test_place_order(ib_client):
    # Arrange
    order = IBOrder()

    # Act
    ib_client.order_manager.place_order(order)

    # Assert
    assert ib_client._eclient.mock_placeOrder.assert_called_with(
        order.orderId,
        order.contract,
        order,
    )


def test_place_order_list(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_cancel_order(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_cancel_all_orders(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_get_open_orders(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_next_order_id(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_process_order_status(ib_client):
    # Arrange

    # Act

    # Assert
    pass
